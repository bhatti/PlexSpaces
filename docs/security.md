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

### Link, monitor, unlink, demonitor

These operations use the caller’s **`RequestContext`** (JWT-derived tenant when auth is enabled, namespace from the request boundary). On each node, **`ActorRegistry::validate_link_monitor_operand_scope`** runs only for operands **hosted on that node**: namespace on **`ActorId`** must match **`RequestContext::namespace`**; when **auth is enabled**, the actor must appear under **`(tenant_id, namespace, actor_id)`** in the local registry (live sender or registered index entry). **`link` / `unlink`** mirror **`monitor` / `demonitor`**: validate and mutate locals inside the local branch; read **`ActorService`** only in remote **`else`** branches. Remote-only operands are enforced when the peer handles the forwarded RPC. This is **orthogonal** to **`ActorVisibility`** (**`ActorRef::tell` / `ask`**).

### Tenant Isolation

Tenant isolation is **mandatory** in PlexSpaces. All operations require a `tenant_id`:

- **RequestContext**: Go-style context carries tenant_id and namespace through call chain
- **Repository Pattern**: All repository methods require RequestContext
- **Service Layer**: All service methods require RequestContext
- **SQL Queries**: All queries automatically filter by tenant_id and namespace

### Multi-tenancy Architecture

PlexSpaces implements a **two-level isolation model**:

1. **Tenant-id** (Primary isolation)
   - **Source of Truth**: JWT token (HTTP) or mTLS certificate (gRPC)
   - **Purpose**: Tenant-level data isolation
   - **When empty**: Only allowed when auth is disabled (`PLEXSPACES_DISABLE_AUTH=1`)
   
2. **Namespace** (Sub-tenant isolation)  
   - **Source of Truth**: Application (when actor is part of an app) or Actor (direct creation)
   - **Purpose**: Allows tenants to create multiple isolated environments (e.g., "production", "staging")
   - **Storage**: Stored in `ActorRef.namespace` and `Actor.namespace` proto field
   - **When empty**: Allowed (represents default namespace within tenant)

```
┌─────────────────────────────────────────────────────────────────┐
│                        Tenant A                                  │
│  ┌─────────────────────┐  ┌─────────────────────┐               │
│  │  Namespace: prod    │  │  Namespace: staging │               │
│  │  - App1             │  │  - App1 (test)      │               │
│  │  - App2             │  │  - App2 (test)      │               │
│  └─────────────────────┘  └─────────────────────┘               │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                        Tenant B                                  │
│  ┌─────────────────────┐                                        │
│  │  Namespace: default │                                        │
│  │  - MyApp            │                                        │
│  └─────────────────────┘                                        │
└─────────────────────────────────────────────────────────────────┘
```

### RequestContext Flow

```
HTTP Request                    gRPC Request
    │                               │
    ▼                               ▼
JWT Middleware              mTLS/JWT Middleware
    │                               │
    ├── Extract tenant_id ──────────┤
    │   (from JWT claims)           │
    │                               │
    ▼                               ▼
┌────────────────────────────────────────────────────┐
│               RequestContext                        │
│  - tenant_id: from auth (JWT/mTLS)                 │
│  - namespace: from application/actor or param      │
│  - user_id: from JWT claims                        │
│  - request_id: generated (ULID)                    │
└────────────────────────────────────────────────────┘
    │
    ▼
Service Layer → Repository Layer → Database Query
    │                                   │
    └───────────────────────────────────┘
      All queries filter by tenant_id + namespace
```

### Namespace in ActorRef

The `ActorRef` stores the actor's namespace:

```rust
pub struct ActorRef {
    id: ActorId,
    namespace: String,  // Source of truth for namespace
    inner: ActorRefInner,
    // ...
}

// Get RequestContext with tenant from auth and namespace from ActorRef
let ctx = actor_ref.get_request_context(tenant_id);
```

### Services That Take Namespace Explicitly

For services not tied to a specific actor (locks, registry, keyvalue), namespace is passed as a parameter:

```rust
// Lock service - namespace as parameter
lock_manager.acquire_lock(&ctx, options).await?;

// KeyValue service - namespace as parameter  
kv_store.get(&ctx, namespace, key).await?;

// Registry service - namespace as parameter
registry.register(&ctx, namespace, key, value).await?;
```

Tenant filtering is **always** applied to these services based on `ctx.tenant_id()`.

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
      cert_dir: "/app/certs"  # Directory for auto-generated certs (or use PLEXSPACES_MTLS_CERT_DIR env var)
      # Or specify paths for production (or use env vars):
      # ca_certificate_path: "/certs/ca.crt"  # Or PLEXSPACES_MTLS_CA_CERT
      # server_certificate_path: "/certs/server.crt"  # Or PLEXSPACES_MTLS_SERVER_CERT
      # server_key_path: "/certs/server.key"  # Or PLEXSPACES_MTLS_SERVER_KEY
```

### Certificate Path Configuration

**Priority for Certificate Paths:**
1. Environment variables (recommended):
   - `PLEXSPACES_MTLS_CA_CERT` - CA certificate path
   - `PLEXSPACES_MTLS_SERVER_CERT` - Server certificate path
   - `PLEXSPACES_MTLS_SERVER_KEY` - Server private key path
   - `PLEXSPACES_MTLS_CERT_DIR` - Directory for auto-generated certs (default: `/app/certs`)
2. Config file paths (fallback)

### Auto-Generation

For local development, PlexSpaces can auto-generate certificates using `rcgen`:

```yaml
runtime:
  security:
    mtls:
      enable_mtls: true
      auto_generate: true
      cert_dir: "/app/certs"  # Or set PLEXSPACES_MTLS_CERT_DIR env var
```

Auto-generated certificates:
- **CA Certificate**: `{cert_dir}/ca.crt` (valid for 1 year)
- **CA Private Key**: `{cert_dir}/ca.key`
- **Server Certificate**: `{cert_dir}/server.crt` (valid for 90 days, signed by CA)
- **Server Private Key**: `{cert_dir}/server.key`

**⚠️ Security Note**: Auto-generated certificates are for development/testing only. Production should use proper certificate management (cert-manager, Vault, AWS Certificate Manager, etc.).

**Validation:**
- If mTLS is enabled and certificates don't exist (and auto_generate is false), node will fail to start with a fatal error
- If `auto_generate: true`, certificates will be generated automatically on first start

### Certificate Rotation

**TODO**: Certificate rotation is planned but not yet implemented.

Configure certificate rotation interval (for future implementation):

```yaml
runtime:
  security:
    mtls:
      certificate_rotation_interval: "720h"  # Rotate every 30 days (planned)
```

**Planned Features:**
- Automatic certificate rotation based on `certificate_rotation_interval`
- Certificate renewal before expiration
- Background task to monitor certificate expiration
- Graceful rotation (generate new cert, update config, restart connections)
- Metrics for certificate rotation events

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

JWT authentication provides secure access to user-facing APIs:

- **HTTP API**: JWT validated at the HTTP gateway; `tenant_id` comes **only** from validated JWT claims (never from client-sent headers like `x-tenant-id`).
- **gRPC**: When gRPC middleware is enabled, JWT or mTLS validates the request; middleware sets `x-tenant-id` from JWT/mTLS only.

The JWT middleware:

- Validates JWT signature and expiration
- Supports **ES256** (ECDSA P-256, asymmetric, preferred) and **HS256** (HMAC-SHA256, symmetric, legacy fallback)
- Extracts `tenant_id`, `roles`, `groups`, and `is_admin` from claims
- Sets request metadata from claims only (prevents header injection)
- Propagates tenant context via RequestContext
- Exposes a **JWKS endpoint** (`GET /.well-known/jwks.json`) for ES256 public key discovery

### JWT Claims (HTTP and gRPC)

Standard and custom claims used by PlexSpaces:

| Claim      | Type    | Required | Description |
|-----------|---------|----------|-------------|
| `sub`     | string  | Yes      | Subject (user ID) |
| `exp`     | number  | Yes      | Expiration time (Unix seconds) |
| `iat`     | number  | Yes      | Issued at (Unix seconds) |
| `tenant_id` | string | Yes    | Tenant identifier (required for multi-tenant isolation) |
| `roles`   | array   | No       | Role names for RBAC |
| `groups`  | array   | No       | Group names |
| `is_admin`| boolean | No      | Admin privilege flag |

**Security**: When auth is enabled, `tenant_id` must come from JWT (or mTLS for gRPC). Client-provided `x-tenant-id` is **not** trusted; the gateway/middleware overwrites it from the validated token.

### Creating JWT Tokens (CLI)

Use the PlexSpaces CLI to create tokens for local or integration testing:

```bash
# Create a JWT (secret from env or --secret)
export PLEXSPACES_JWT_SECRET=your-secret-key
cargo run -p plexspaces-cli -- jwt create \
  --tenant-id internal \
  --sub system \
  --roles admin \
  --groups "ops,dev" \
  --is-admin \
  --exp-hours 1

# Or pass secret explicitly (avoid in scripts)
cargo run -p plexspaces-cli -- jwt create \
  --tenant-id my-tenant \
  --sub user@example.com \
  --roles "read,write" \
  --secret "$PLEXSPACES_JWT_SECRET" \
  --exp-hours 24
```

The CLI prints the token and a usage hint. Use it in HTTP requests as `Authorization: Bearer <token>`.

### Configuration

Configure JWT in your `release.yaml` (or `release-auth.yaml`):

```yaml
runtime:
  security:
    jwt:
      enable_jwt: true
      algorithm: "ES256"              # ES256 (preferred) or HS256 (legacy)
      private_key_pem: ""             # Inline PEM → reads PLEXSPACES_JWT_PRIVATE_KEY env var
      private_key_file: ""            # Path to PEM → reads PLEXSPACES_JWT_PRIVATE_KEY_FILE env var
      auto_generate_key: true         # Auto-generate ES256 key on first start (dev mode)
      secret: ""                      # HS256 fallback → reads PLEXSPACES_JWT_SECRET env var
      issuer: "plexspaces"
      jwks_url: ""                    # External JWKS URL for validation (optional)
      allowed_audiences:
        - "plexspaces-api"
      tenant_id_claim: "tenant_id"
      user_id_claim: "sub"
```

### Key Management

**ES256 (Asymmetric, Preferred):**

PlexSpaces uses ECDSA P-256 (ES256) by default. The private key signs tokens; the corresponding public key verifies them and is exposed via the JWKS endpoint.

**Key Resolution Order** (first match wins):
1. `PLEXSPACES_JWT_PRIVATE_KEY` env var — inline PEM string (for Docker/K8s secrets)
2. `PLEXSPACES_JWT_PRIVATE_KEY_FILE` env var or `private_key_file` config — path to PEM file
3. Auto-generate: if `auto_generate_key: true` and no key exists, generate a new P-256 key pair and save to the configured file path
4. `PLEXSPACES_JWT_SECRET` env var — fall back to HS256 symmetric signing

**Generate a key manually:**
```bash
openssl ecparam -genkey -name prime256v1 -noout -out jwt-es256.pem
```

**JWKS Endpoint:** `GET /.well-known/jwks.json` returns the ES256 public key in JWK format. External services can use this to verify PlexSpaces-issued tokens without sharing secrets.

**HS256 (Symmetric, Legacy Fallback):**

If only `PLEXSPACES_JWT_SECRET` is set (and no private key is available), PlexSpaces falls back to HS256 symmetric signing. This requires all verifiers to share the same secret.

```bash
export PLEXSPACES_JWT_SECRET="your-secret-key-at-least-32-chars"
```

**Validation:**
- If JWT is enabled and no key material is available (no private key, no secret, auto-generate disabled), the HTTP gateway returns 503
- If `PLEXSPACES_DISABLE_AUTH=1` is set, auth is disabled (testing only)

**Multi-Node Clusters:**
All nodes in a cluster must share the same JWT signing key so tokens issued by one node are valid on all others. The default `scripts/server.sh` uses a shared path (`./certs/jwt-es256.pem`) regardless of port. In production, distribute the same PEM file to all nodes (via Kubernetes secrets, Vault, etc.) or set `PLEXSPACES_JWT_PRIVATE_KEY` from a shared secret store.

### Tenant Extraction (No Client-Sent tenant_id When Auth On)

When authentication is **enabled**:

- **HTTP**: The gateway validates the `Authorization: Bearer <token>` header and derives `tenant_id` (and optionally `roles`, `groups`, `is_admin`) from the JWT only. Path parameters or headers like `x-tenant-id` from the client are **not** used for tenant identity.
- **gRPC**: The auth middleware (when used) validates JWT or mTLS and sets `x-tenant-id` (and related headers) from the token or certificate only. Downstream code reads these headers; they are never taken from the raw client request.

When authentication is **disabled** (e.g. `PLEXSPACES_DISABLE_AUTH=1`), the gateway may use a local default tenant for testing. Client-supplied tenant headers are not part of the public contract.

The middleware sets (from JWT only when auth on):

- `x-tenant-id`: From JWT `tenant_id` claim
- `x-user-id`: From JWT `sub` claim
- `x-user-roles`: Comma-separated from JWT `roles`
- `x-user-groups`: Comma-separated from JWT `groups`
- `x-admin`: From JWT `is_admin` claim

### Token Validation

JWT tokens are validated against:

1. **Algorithm**: Pinned to the configured key type (ES256 for EC key pair, HS256 for shared secret). The token’s `alg` header is not trusted (algorithm confusion prevention).
2. **Signature**: Verified with the EC public key (ES256) or shared secret (HS256).
3. **Expiration**: `exp` is checked (tokens are rejected when expired).
4. **Issuer** (if configured)
5. **Audience** (if configured)
6. **tenant_id**: Must be non-empty in claims for request context creation.

### Example JWT Token (Claims)

```json
{
  "sub": "user-123",
  "tenant_id": "tenant-456",
  "roles": ["admin", "user"],
  "groups": ["ops"],
  "is_admin": true,
  "exp": 1735689600,
  "iat": 1735603200
}
```

### Auth Error Messages and Local Testing

When a request is rejected due to missing or invalid auth, the response body includes an actionable hint:

- **HTTP 401 (Unauthorized)**: Missing or invalid JWT. Message includes guidance to provide a valid `Authorization: Bearer <token>` and, for local testing, to set `PLEXSPACES_DISABLE_AUTH=1`.
- **HTTP 503 (Service Unavailable)**: Auth is enabled but JWT secret is not configured. Message directs you to set `PLEXSPACES_JWT_SECRET` (or configure `security.jwt.secret`) or to disable auth for local testing.

**Local testing without auth:**

```bash
export PLEXSPACES_DISABLE_AUTH=1
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093
```

Then call HTTP or gRPC without a token (when the node is configured to allow it).

## OAuth/OIDC Authentication

### Overview

PlexSpaces supports OAuth 2.0 / OpenID Connect (OIDC) for user authentication. Users authenticate via any OIDC-compliant provider (Google, Okta, Auth0, Keycloak, etc.), and PlexSpaces issues a short-lived session JWT stored in an HttpOnly cookie.

### How It Works

```
Browser                PlexSpaces Node              OIDC Provider
  │                        │                            │
  │  GET /api/v1/auth/oidc/login                        │
  ├───────────────────────►│                            │
  │                        │  302 Redirect to provider  │
  │◄───────────────────────┤                            │
  │                        │                            │
  │  (User authenticates at provider)                   │
  │────────────────────────────────────────────────────►│
  │                        │                            │
  │  GET /api/v1/auth/oidc/callback?code=...&state=...  │
  │◄────────────────────────────────────────────────────│
  ├───────────────────────►│                            │
  │                        │  Exchange code for tokens  │
  │                        ├───────────────────────────►│
  │                        │◄───────────────────────────│
  │                        │                            │
  │                        │  Extract claims, upsert user/tenant
  │                        │  Issue session JWT (15m TTL)
  │  Set-Cookie: psx_session=<JWT>; HttpOnly; Secure    │
  │◄───────────────────────┤                            │
  │  302 → /dashboard      │                            │
```

### Configuration

```yaml
security:
  disable_auth: false
  oidc:
    enabled: true
    discovery_url: "https://accounts.google.com/.well-known/openid-configuration"
    client_id: "YOUR_CLIENT_ID"          # or PLEXSPACES_OIDC_CLIENT_ID env var
    client_secret: "YOUR_SECRET"         # or PLEXSPACES_OIDC_CLIENT_SECRET env var
    redirect_uri: "http://localhost:8091/api/v1/auth/oidc/callback"
    scopes: ["openid", "email", "profile"]
    tenant_claim: "hd"                   # OIDC claim that maps to tenant slug
    admin_groups: ["platform-admins"]    # OIDC groups that grant admin privilege
    default_tenant_id: "default"         # Fallback when tenant_claim is absent
  jwt:
    enable_jwt: true
    secret: ""                           # Use PLEXSPACES_JWT_SECRET env var
    issuer: "plexspaces"
    token_ttl: "15m"
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `PLEXSPACES_OIDC_CLIENT_ID` | OAuth client ID (overrides config) |
| `PLEXSPACES_OIDC_CLIENT_SECRET` | OAuth client secret (overrides config) |
| `PLEXSPACES_JWT_PRIVATE_KEY` | Inline ES256 PEM private key (preferred) |
| `PLEXSPACES_JWT_PRIVATE_KEY_FILE` | Path to ES256 PEM file (auto-generates if missing) |
| `PLEXSPACES_JWT_SECRET` | HS256 fallback secret for signing session JWTs |
| `PLEXSPACES_DISABLE_AUTH` | Set to `1` to disable all auth (dev only) |

### Provider Examples

**Google Workspace:**
```yaml
oidc:
  discovery_url: "https://accounts.google.com/.well-known/openid-configuration"
  tenant_claim: "hd"    # Google hosted domain → tenant slug
  scopes: ["openid", "email", "profile"]
```
Setup: [Google Cloud Console](https://console.cloud.google.com/apis/credentials) → Create OAuth 2.0 Client ID → Add redirect URI.

**Okta:**
```yaml
oidc:
  discovery_url: "https://YOUR_ORG.okta.com/.well-known/openid-configuration"
  tenant_claim: "org"
  scopes: ["openid", "email", "profile", "groups"]
```

**Auth0:**
```yaml
oidc:
  discovery_url: "https://YOUR_DOMAIN.auth0.com/.well-known/openid-configuration"
  tenant_claim: "org_id"
  scopes: ["openid", "email", "profile"]
```

**Keycloak (self-hosted):**
```yaml
oidc:
  discovery_url: "http://localhost:8180/realms/plexspaces/.well-known/openid-configuration"
  tenant_claim: "org"
  scopes: ["openid", "email", "profile"]
```

### User → Tenant Mapping

When a user authenticates via OIDC:

1. The `tenant_claim` (e.g., `hd`, `org`, `org_id`) is extracted from the ID token claims.
2. `get_or_create_tenant(slug)` creates the tenant if it doesn't exist (idempotent upsert).
3. `get_or_create_user(email, tenant_id)` creates or updates the user record.
4. A session JWT is issued with `tenant_id`, `sub`, `roles`, `is_admin`, and stored as an HttpOnly cookie.

If the `tenant_claim` is absent from the ID token, the `default_tenant_id` from config is used.

### Admin Detection

Users are granted admin privileges when their OIDC groups claim includes any group listed in `admin_groups`:

```yaml
oidc:
  admin_groups: ["platform-admins", "infra-team"]
```

The `is_admin` flag is stored on the user record and included in all session JWTs.

---

## API Tokens (Programmatic Access)

### Overview

API tokens provide long-lived programmatic access for CI/CD pipelines, scripts, and SDKs. They follow the GitHub Personal Access Token (PAT) pattern:

- **Format**: `psx_<64 hex chars>` (32 random bytes, hex-encoded)
- **Storage**: Only the SHA-256 hash is stored in the database
- **Display**: Plaintext is shown exactly once at creation time
- **Validation**: Token is hashed → looked up in `api_tokens` table → checked for revocation/expiry

### Creating API Tokens

**Via Dashboard:**
Visit `/dashboard/tokens` → Click "Create Token" → Set name, scopes, and TTL → Copy the plaintext token immediately.

**Via API (with session JWT or existing API token):**
```bash
curl -X POST http://localhost:8091/api/v1/auth/tokens \
  -H "Authorization: Bearer <session-jwt-or-psx-token>" \
  -H "Content-Type: application/json" \
  -d '{"name": "ci-pipeline", "scopes": ["read", "write"], "ttl_seconds": 7776000}'
```

Response:
```json
{
  "token": {
    "token_id": "01J...",
    "name": "ci-pipeline",
    "prefix": "psx_abc1",
    "scopes": ["read", "write"]
  },
  "plaintext": "psx_a1b2c3d4e5f6..."
}
```

### Using API Tokens

Pass the token as a Bearer token in the `Authorization` header:

```bash
# List users
curl -H "Authorization: Bearer psx_a1b2c3d4e5f6..." \
  http://localhost:8091/api/v1/auth/users

# Send a message to an actor
curl -H "Authorization: Bearer psx_a1b2c3d4e5f6..." \
  -X POST http://localhost:8091/api/v1/actors/my-actor/messages \
  -H "Content-Type: application/json" \
  -d '{"payload": "hello"}'
```

### Token Lifecycle

| Operation | Endpoint | Method |
|-----------|----------|--------|
| Create | `/api/v1/auth/tokens` | POST |
| List (own tokens) | `/api/v1/auth/tokens` | GET |
| Revoke | `/api/v1/auth/tokens/:token_id` | DELETE |

### Security Properties

- **`is_admin` snapshotted at creation**: The token carries the admin status of the user at creation time. If a user is later demoted, existing tokens retain their original privilege level (revoke and re-create to reflect new permissions).
- **SHA-256 stored, plaintext never persisted**: Even if the database is compromised, tokens cannot be recovered.
- **`last_used_at` tracking**: Updated asynchronously on each use for audit purposes.
- **Scoped**: Tokens carry explicit scopes (e.g., `read`, `write`) checked by handlers.

### Unified Auth Design

All three authentication methods produce the same downstream `JwtClaims` interface:

```
┌──────────────────────────────────────────────────────────┐
│                    Incoming Request                        │
│                                                          │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │ Session JWT │  │ psx_ Token  │  │ mTLS Certificate│  │
│  │ (cookie)    │  │ (Bearer)    │  │ (gRPC node)     │  │
│  └──────┬──────┘  └──────┬──────┘  └────────┬────────┘  │
│         │                │                   │           │
│         ▼                ▼                   ▼           │
│  ┌──────────────────────────────────────────────────┐    │
│  │              JwtClaims (uniform)                  │    │
│  │  sub, tenant_id, roles, groups, is_admin         │    │
│  └──────────────────────────────────────────────────┘    │
│         │                                                │
│         ▼                                                │
│  ┌──────────────────────────────────────────────────┐    │
│  │           Downstream Handlers                     │    │
│  │  (auth-method-agnostic, same code path)           │    │
│  └──────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────┘
```

---

## Testing Authentication

### Development Mode (Auth Disabled)

For local development, disable auth entirely:

```bash
export PLEXSPACES_DISABLE_AUTH=1
cargo run -p plexspaces-cli -- start --node-id dev-node --listen-addr 0.0.0.0:8091
```

All endpoints return data as superadmin — no login or token needed:

```bash
curl http://localhost:8091/api/v1/auth/users
curl http://localhost:8091/api/v1/auth/tenants
curl -X POST http://localhost:8091/api/v1/auth/tokens \
  -H "Content-Type: application/json" \
  -d '{"name": "test-token"}'
```

### Testing OIDC Login Flow

1. Configure OIDC provider credentials (see [Provider Examples](#provider-examples) above).
2. Start the node with auth enabled.
3. Open browser: `http://localhost:8091/api/v1/auth/oidc/login`
4. Authenticate with your provider.
5. Verify redirect to `/dashboard` with session cookie set.

### Testing API Tokens End-to-End

```bash
# 1. Create a token (requires active session or auth disabled)
TOKEN_RESPONSE=$(curl -s -X POST http://localhost:8091/api/v1/auth/tokens \
  -H "Content-Type: application/json" \
  -d '{"name": "e2e-test", "scopes": ["read","write"], "ttl_seconds": 3600}')

PLAINTEXT=$(echo "$TOKEN_RESPONSE" | jq -r '.plaintext')
TOKEN_ID=$(echo "$TOKEN_RESPONSE" | jq -r '.token.token_id')

# 2. Use the token
curl -H "Authorization: Bearer $PLAINTEXT" \
  http://localhost:8091/api/v1/auth/users

# 3. List tokens
curl -H "Authorization: Bearer $PLAINTEXT" \
  http://localhost:8091/api/v1/auth/tokens

# 4. Revoke the token
curl -H "Authorization: Bearer $PLAINTEXT" \
  -X DELETE http://localhost:8091/api/v1/auth/tokens/$TOKEN_ID

# 5. Verify revoked token is rejected (should return 401)
curl -H "Authorization: Bearer $PLAINTEXT" \
  http://localhost:8091/api/v1/auth/users
```

### Testing Access Control

```bash
# Non-admin user: only sees own tenant's data
curl -H "Authorization: Bearer $USER_TOKEN" \
  http://localhost:8091/api/v1/auth/users
# → Returns only users in the token holder's tenant

# Admin user: sees all tenants
curl -H "Authorization: Bearer $ADMIN_TOKEN" \
  http://localhost:8091/api/v1/auth/users
# → Returns all users across all tenants (paginated)

# Non-admin attempting to filter by another tenant (ignored):
curl -H "Authorization: Bearer $USER_TOKEN" \
  "http://localhost:8091/api/v1/auth/users?tenant_id=other-tenant"
# → Still returns only own tenant's users (filter silently overridden)
```

### Dashboard UI Testing

With the node running, visit:
- `http://localhost:8091/dashboard/tokens` — Create, view, and revoke API tokens
- `http://localhost:8091/dashboard/users` — Browse users and tenants with pagination

---

## Tenant Isolation

### RequestContext

All operations require a `RequestContext`:

```rust
use plexspaces_actor::RequestContext;

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

All SQL queries automatically filter by tenant_id and namespace:

```sql
-- ✅ Correct: Filters by tenant_id and namespace
SELECT * FROM blob_metadata
WHERE blob_id = $1 AND tenant_id = $2 AND namespace = $3

-- ✅ Correct: Admin/internal context with empty namespace (skips namespace filter)
SELECT * FROM blob_metadata
WHERE blob_id = $1 AND tenant_id = $2
-- Note: Namespace filter skipped when ctx.should_skip_namespace_filter() is true

-- ❌ Wrong: Missing tenant_id filter
SELECT * FROM blob_metadata
WHERE blob_id = $1
```

### Namespace Filtering

Repository lookup methods (read-only operations) support conditional namespace filtering:

- **Normal Contexts**: Always filter by both `tenant_id` and `namespace`
- **Admin/Internal Contexts with Empty Namespace**: Skip namespace filtering to allow cross-namespace queries

```rust
// Check if namespace filtering should be skipped
if ctx.should_skip_namespace_filter() {
    // Admin or internal context with empty namespace
    // Query without namespace filter (cross-namespace query)
    sqlx::query("SELECT * FROM kv_store WHERE tenant_id = ? AND key = ?")
        .bind(ctx.tenant_id())
        .bind(key)
        .fetch_optional(&pool)
        .await?
} else {
    // Normal context - filter by namespace
    sqlx::query("SELECT * FROM kv_store WHERE tenant_id = ? AND namespace = ? AND key = ?")
        .bind(ctx.tenant_id())
        .bind(ctx.namespace())
        .bind(key)
        .fetch_optional(&pool)
        .await?
}
```

**Important**: Write operations (put, delete, update) always filter by namespace to maintain data isolation.

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

For integration tests or local development, you can disable auth:

```bash
export PLEXSPACES_DISABLE_AUTH=1
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093
```

Or in Docker/Compose:

```yaml
# docker-compose.test.yml
services:
  plexspaces-node:
    environment:
      - PLEXSPACES_DISABLE_AUTH=1  # Only for integration tests / local dev
```

**WARNING**: Never disable auth in production!

### Integration Test Setup

```rust
use plexspaces_actor::RequestContext;

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

### Security Headers (Set by Middleware from JWT/mTLS Only)

When auth is enabled, these headers are set **only** by the gateway/middleware from the validated JWT (or mTLS); client-sent values are ignored or overwritten:

- `x-tenant-id`: Tenant identifier (from JWT `tenant_id` or mTLS)
- `x-user-id`: User identifier (from JWT `sub`)
- `x-user-roles`: Comma-separated roles (from JWT `roles`)
- `x-user-groups`: Comma-separated groups (from JWT `groups`)
- `x-admin`: Admin flag (from JWT `is_admin`)
- `x-request-id`: Request ID for tracing (generated)

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

## Middleware and Observability

### HTTP Gateway (JWT)

The HTTP gateway applies JWT auth middleware to `/api/v1/actors/*` when auth is enabled:

- **Middleware**: Validates `Authorization: Bearer <token>`, extracts claims, and sets `HttpJwtClaims` in request extensions. Downstream handlers use tenant_id from JWT only (not from path or client headers when auth is on).
- **Observability**: On auth failure, the middleware logs without exposing tokens:
  - **503**: `tracing::warn!(path, "HTTP auth: JWT secret not configured (returning 503)")`
  - **401**: `tracing::debug!(path, has_bearer, "HTTP auth: JWT validation failed (401)")`

### gRPC Middleware (plexspaces-grpc-middleware)

The `plexspaces-grpc-middleware` crate provides production-ready interceptors:

- **AuthInterceptor**: JWT or mTLS; validates token/certificate, enforces RBAC, and sets metadata from claims only. Algorithm is pinned (HS256 for shared secret, RS256 for RSA) to prevent algorithm-confusion attacks.
- **InterceptorChain**: Composable chain built from config (metrics, auth, rate limit, tracing, etc.). Auth interceptor removes any client-sent `x-tenant-id` and sets it from JWT/mTLS only.
- **Metrics, Tracing, Rate Limit**: Additional interceptors for observability and protection.

Full integration of the gRPC InterceptorChain into the node server (so every gRPC request runs through the chain) is done when the node is built with middleware config; the chain and auth interceptor are implemented and tested in the grpc-middleware crate.

#### gRPC tenant-id vs namespace (caller contract)

- **Tenant-id**: Callers do **not** choose tenant by setting metadata themselves when JWT auth is enabled. The JWT proves identity; middleware writes `x-tenant-id` (and related identity headers) from validated claims. Treat any pre-middleware client value as untrusted; production paths rely on `AuthInterceptor` clearing and repopulating metadata.
- **Namespace**: `x-namespace` may be supplied for request scoping (or taken from proto fields / labels where documented). Services build `RequestContext` via `request_context_from_grpc_request` so tenant and namespace follow the same validation rules as HTTP.
- **Services**: gRPC handlers should use `request_context_from_grpc_request` (or equivalent) for every RPC that touches tenant-scoped state. Avoid ad hoc `RequestContext::new_without_auth("", "")` on code paths that perform routing or registry access.
- **WASM host routing**: When a WASM actor sends or asks, the host must derive `RequestContext` from the registered sender actor/runtime metadata instead of manufacturing an empty tenant. That preserves the tenant established by JWT/mTLS and the namespace already bound to the actor identity.

### RequestContext and Auth

- **Auth enabled**: `request_context_from_grpc_request` uses `auth_enabled = !service_locator.is_auth_disabled().await`. Tenant_id must be present in metadata (set by auth middleware from JWT/mTLS). If missing, errors include a hint: *"Authentication required: provide a valid JWT (HTTP) or use mTLS (gRPC). For local testing, set PLEXSPACES_DISABLE_AUTH=1."*
- **Auth disabled**: Tenant_id may come from metadata or defaults for local testing.

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

# JWT (ES256 preferred, HS256 fallback)
PLEXSPACES_JWT_ENABLED=true
PLEXSPACES_JWT_PRIVATE_KEY=...          # Inline ES256 PEM (for Docker/K8s secrets)
PLEXSPACES_JWT_PRIVATE_KEY_FILE=...     # Path to ES256 PEM file
PLEXSPACES_JWT_SECRET=...               # HS256 fallback secret (if no EC key configured)
PLEXSPACES_JWT_ISSUER=https://auth.example.com
PLEXSPACES_JWT_JWKS_URL=https://auth.example.com/.well-known/jwks.json
PLEXSPACES_JWT_TENANT_ID_CLAIM=tenant_id

# Security
PLEXSPACES_DISABLE_AUTH=false  # Set to true to disable auth for testing (development only)
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

## Auth Headers & Security Schemes (OpenAPI-Style)

### Overview

PlexSpaces supports propagating authentication credentials and custom headers through the
entire call chain via the `RequestContext.headers` field. This follows the same patterns
defined by [OpenAPI 3.0 Security Schemes](https://swagger.io/docs/specification/authentication/),
allowing actors and services to make authenticated outbound calls using credentials
propagated from the original request.

### Supported Security Schemes

PlexSpaces maps to OpenAPI security scheme types:

| OpenAPI Scheme                     | PlexSpaces API                          | Header Key           | Example Value                 |
|------------------------------------|-----------------------------------------|----------------------|-------------------------------|
| `type: http, scheme: bearer`       | `ctx.with_bearer_token(token)`          | `authorization`      | `Bearer eyJhbGci...`         |
| `type: apiKey, in: header`         | `ctx.with_api_key_header(name, key)`    | `x-api-key` (or custom) | `sk-abc123`              |
| `type: apiKey, in: query`          | `ctx.with_api_key_query(name, key)`     | `apikey-query:<name>` | `sk-abc123`                  |
| `type: mutualTLS`                  | mTLS at transport layer                 | (TLS, not header)    | Certificate-based             |
| Custom headers                     | `ctx.with_header(name, value)`          | any lowercase name   | any value                     |

### OpenAPI Security Definitions (Proto-Generated)

Security schemes are defined in proto files using `protoc-gen-openapiv2` annotations and
are automatically included in the generated OpenAPI spec. From `common.proto`:

```protobuf
option (grpc.gateway.protoc_gen_openapiv2.options.openapiv2_swagger) = {
  // ...
  security_definitions: {
    security: {
      key: "BearerAuth";
      value: {
        type: TYPE_API_KEY;
        in: IN_HEADER;
        name: "Authorization";
        description: "JWT Bearer token. Format: Bearer <token>.";
      };
    };
    security: {
      key: "ApiKeyHeader";
      value: {
        type: TYPE_API_KEY;
        in: IN_HEADER;
        name: "X-API-Key";
        description: "API key authentication via header.";
      };
    };
    security: {
      key: "ApiKeyQuery";
      value: {
        type: TYPE_API_KEY;
        in: IN_QUERY;
        name: "api_key";
        description: "API key via query parameter.";
      };
    };
  };
  security: {
    security_requirement: {
      key: "BearerAuth";
      value: {};
    };
  };
};
```

This generates the standard OpenAPI `securityDefinitions` / `security` blocks when
`protoc-gen-openapiv2` runs. The generated spec (`docs/openapi.json`) will include:

```json
{
  "securityDefinitions": {
    "BearerAuth": { "type": "apiKey", "name": "Authorization", "in": "header" },
    "ApiKeyHeader": { "type": "apiKey", "name": "X-API-Key", "in": "header" },
    "ApiKeyQuery": { "type": "apiKey", "name": "api_key", "in": "query" }
  },
  "security": [{ "BearerAuth": [] }]
}
```

### Rust API

#### Attaching Credentials

```rust
use plexspaces_common::RequestContext;

// Bearer token (most common — JWT)
let ctx = RequestContext::new("tenant-123".to_string(), "prod".to_string(), true)?
    .with_bearer_token("eyJhbGciOiJIUzI1NiIs...".to_string());

// API key in header
let ctx = RequestContext::new("tenant-123".to_string(), "prod".to_string(), false)?
    .with_api_key_header("x-api-key".to_string(), "sk-live-abc123".to_string());

// API key in query parameter
let ctx = RequestContext::new("tenant-123".to_string(), "prod".to_string(), false)?
    .with_api_key_query("api_key".to_string(), "sk-live-abc123".to_string());

// Multiple credentials + custom headers
let ctx = RequestContext::new("tenant-123".to_string(), "prod".to_string(), true)?
    .with_bearer_token("eyJ...".to_string())
    .with_header("x-request-source".to_string(), "mobile-app".to_string())
    .with_header("x-idempotency-key".to_string(), "req-abc-123".to_string());

// Bulk headers (e.g., from inbound HTTP request)
let mut headers = std::collections::HashMap::new();
headers.insert("authorization".to_string(), "Bearer eyJ...".to_string());
headers.insert("x-api-key".to_string(), "sk-abc".to_string());
let ctx = RequestContext::new("tenant-123".to_string(), "prod".to_string(), false)?
    .with_headers(headers);
```

#### Reading Credentials

```rust
// Bearer token (strips "Bearer " prefix)
if let Some(token) = ctx.bearer_token() {
    // token = "eyJhbGciOiJIUzI1NiIs..."
}

// Any header by name
if let Some(api_key) = ctx.get_header("x-api-key") {
    // api_key = "sk-live-abc123"
}

// API key query parameter
if let Some(key) = ctx.api_key_query("api_key") {
    // key = "sk-live-abc123"
}

// All HTTP headers (excludes internal apikey-query:* entries)
let http_headers = ctx.http_headers();
// Returns: {"authorization": "Bearer ...", "x-api-key": "sk-..."}

// All query-param API keys
let query_params = ctx.api_key_query_params();
// Returns: {"api_key": "sk-..."}
```

#### Creating Context from Auth with Headers

```rust
// From validated JWT + inbound request headers
let ctx = RequestContext::from_auth_with_headers(
    Some("tenant-123".to_string()),  // from JWT
    Some("prod".to_string()),        // from request path
    Some("user-456".to_string()),    // from JWT sub
    false,                           // admin
    true,                            // auth_enabled
    None,                            // default_tenant_id
    None,                            // default_namespace
    inbound_headers,                 // propagate from request
)?;
```

### Proto Wire Format

The `RequestContext` proto message includes a `headers` map (field 11):

```protobuf
message RequestContext {
  string tenant_id = 1;
  string namespace = 2;
  string user_id = 3;
  string request_id = 4;
  string correlation_id = 5;
  google.protobuf.Timestamp timestamp = 6;
  map<string, string> metadata = 7;
  bool admin = 8;
  bool internal = 9;
  bool auth_enabled = 10;
  map<string, string> headers = 11;  // Auth/credential propagation
}
```

Headers survive proto serialization roundtrips:

```rust
let original = RequestContext::new_without_auth("t1".into(), "ns".into())
    .with_bearer_token("tok".to_string());

let proto = original.to_proto();
let restored = RequestContext::from_proto(&proto, false).unwrap();

assert_eq!(restored.bearer_token(), Some("tok"));
```

### WIT (WASM Actors)

The WIT `context` record includes a `headers` field for WASM actors:

```wit
record context {
    tenant-id: string,
    namespace: string,
    headers: list<tuple<string, string>>,
}
```

WASM actors can read and propagate auth headers when making outbound calls:

```python
# Python SDK example
from plexspaces import host

def handle_message(ctx, msg):
    # Read bearer token from context
    for name, value in ctx.headers:
        if name == "authorization":
            # Use token for outbound HTTP call
            host.http_request("https://api.example.com/data",
                              headers={"Authorization": value})
```

```typescript
// TypeScript SDK example
import { Host } from "plexspaces";

function handleMessage(ctx: Context, msg: Message) {
    const authHeader = ctx.headers.find(([k]) => k === "authorization");
    if (authHeader) {
        // Propagate to downstream service
        Host.tell(targetActor, msgType, payload, {
            headers: [authHeader]
        });
    }
}
```

### Header Propagation Flow

```
Client Request
    │
    ├── Authorization: Bearer <JWT>
    ├── X-API-Key: sk-abc123
    └── X-Custom: value
         │
         ▼
┌────────────────────────────────┐
│   HTTP Gateway / gRPC Middleware│
│                                │
│  1. Validate JWT/mTLS          │
│  2. Extract tenant_id from JWT │
│  3. Build RequestContext with   │
│     headers from request       │
└────────────────────────────────┘
         │
         ▼
┌────────────────────────────────┐
│       RequestContext            │
│  tenant_id: "tenant-123"       │
│  namespace: "prod"             │
│  headers:                      │
│    authorization: Bearer <JWT> │
│    x-api-key: sk-abc123        │
│    x-custom: value             │
└────────────────────────────────┘
         │
    ┌────┴────┐
    ▼         ▼
 Actor     Service
  │           │
  │  ctx.bearer_token()
  │  ctx.get_header("x-api-key")
  │           │
  ▼           ▼
 Outbound HTTP/gRPC calls
 with propagated headers
```

### Security Considerations

1. **Bearer tokens from JWT only (when auth enabled)**: When authentication is enabled,
   the `authorization` header in `RequestContext.headers` is set from the validated JWT
   token only. Client-supplied `Authorization` headers are stripped by the auth middleware
   before being added to the context. This prevents token injection attacks.

2. **Header names are lowercase**: All header names are stored in lowercase per HTTP/2
   convention. The `with_header()` and `get_header()` methods handle case normalization.

3. **Sensitive headers should not be logged**: When logging `RequestContext`, the `headers`
   field may contain secrets (tokens, API keys). Use the `SecretMasker` utility or exclude
   headers from log output.

4. **Headers propagate across nodes**: When an actor message is forwarded to a remote node,
   `RequestContext` (including headers) is serialized via proto. Ensure auth headers are
   appropriate for cross-node propagation (e.g., internal service tokens vs. user tokens).

5. **API keys in query params are less secure**: The `with_api_key_query()` method stores
   keys internally but they may appear in URLs/logs. Prefer header-based API keys when
   possible.

6. **WASM actor sandbox**: WASM actors can read headers from their context but cannot
   modify the context of their parent. Headers flow downward (caller → callee), never
   upward.

### Comparison with OpenAPI Security Schemes

| Feature                    | OpenAPI 3.0               | PlexSpaces RequestContext         |
|----------------------------|---------------------------|-----------------------------------|
| Bearer token (HTTP)        | `type: http, scheme: bearer` | `with_bearer_token()`          |
| API key in header          | `type: apiKey, in: header` | `with_api_key_header(name, key)` |
| API key in query           | `type: apiKey, in: query`  | `with_api_key_query(name, key)`  |
| Mutual TLS                 | `type: mutualTLS`          | Transport-level mTLS config       |
| OAuth2 / OpenID Connect    | `type: oauth2` / `openIdConnect` | JWT token from OIDC provider → `with_bearer_token()` |
| Custom scheme              | extension (`x-*`)          | `with_header(name, value)`        |
| Multiple schemes (AND)     | `security: [{A, B}]`      | Chain multiple `with_*` calls     |
| Multiple schemes (OR)      | `security: [{A}, {B}]`    | Different code paths per scheme   |
| Scope-based authorization  | `security: [{A: [read]}]` | Roles/scopes in JWT claims        |

## Troubleshooting

### Common Issues

1. **HTTP 401 Unauthorized when calling `/api/v1/actors/...`**
   - **Cause**: Auth is enabled but no valid JWT was provided (or token is expired/invalid).
   - **Solution**: Send `Authorization: Bearer <token>` with a token created via `plexspaces-cli jwt create` (see [Creating JWT Tokens (CLI)](#creating-jwt-tokens-cli)). Or for local testing only: `export PLEXSPACES_DISABLE_AUTH=1` and restart the node.

2. **HTTP 503 when calling `/api/v1/actors/...`**
   - **Cause**: Auth is enabled but JWT secret is not configured.
   - **Solution**: Set `PLEXSPACES_JWT_SECRET` (or configure `security.jwt.secret` in release config). Or for local testing: `export PLEXSPACES_DISABLE_AUTH=1`.

3. **"Missing required tenant_id in RequestContext"**
   - **Cause**: Auth is enabled but tenant_id was not set (e.g. JWT missing `tenant_id` claim or gRPC metadata not set by middleware).
   - **Solution**: Ensure JWT includes a non-empty `tenant_id` claim; ensure gRPC requests go through auth middleware so metadata is set. Error message includes: *"For local testing, set PLEXSPACES_DISABLE_AUTH=1."*

4. **"JWT secret must be an environment variable reference"**
   - **Solution**: Use env var for secret (e.g. `PLEXSPACES_JWT_SECRET`); avoid putting the secret in config files.

5. **"Metadata tenant_id does not match context tenant_id"**
   - **Solution**: Ensure metadata tenant_id matches RequestContext (tenant_id comes from JWT/mTLS only when auth is on).

6. **Certificate validation fails**
   - **Solution**: Check CA certificate is correct and not expired.

## References

- [RequestContext](crates/common/src/request_context.rs) - RequestContext with auth header propagation (see `with_bearer_token()`, `with_api_key_header()`, `with_header()`)
- [Common Proto](proto/plexspaces/v1/common.proto) - RequestContext proto definition with `headers` field (tag 11) and OpenAPI security definitions
- [Security Proto](proto/plexspaces/v1/security/security.proto) - Security API proto with OpenAPI security schemes
- [User Proto](proto/plexspaces/v1/security/user.proto) - User, Tenant, and ApiToken message definitions
- [WIT Types](wit/plexspaces-actor/types.wit) - WIT `context` record with `headers: list<tuple<string, string>>`
- [HTTP JWT validation](crates/node/src/http_jwt.rs) - HTTP gateway JWT validation
- [Auth Routes](crates/node/src/http_routes/auth_routes.rs) - OAuth/OIDC login, API token CRUD, user/tenant listing
- [OIDC Implementation](crates/services/src/user_service/oidc.rs) - OIDC discovery, login redirect, callback handling
- [API Token Repository](crates/services/src/user_service/api_token_repository.rs) - Token storage, validation, revocation
- [Tenant Repository](crates/services/src/user_service/tenant_repository.rs) - Tenant CRUD and upsert
- [gRPC Auth Interceptor](crates/grpc-middleware/src/auth.rs) - JWT/mTLS middleware for gRPC
- [InterceptorChain](crates/grpc-middleware/src/chain.rs) - Composable gRPC middleware chain
- [mTLS Certificate Generation](crates/grpc-middleware/src/cert_gen.rs) - Certificate generation
- [mTLS Server](crates/node/src/tls_server.rs) - TLS accept loop for HTTPS serving
- [gRPC Client TLS](crates/node/src/grpc_client.rs) - `connect_with_tls()` for mTLS outbound connections
- [CLI jwt create](crates/cli/src/security.rs) - CLI helper to create JWT tokens
- [Dashboard: Tokens](crates/dashboard/static/dashboard/tokens.html) - API token management UI
- [Dashboard: Users](crates/dashboard/static/dashboard/users.html) - User/tenant listing UI
