# Quick Start: Starting PlexSpaces Server

This guide shows you how to start a PlexSpaces node server with different configuration options.

## Option 1: Start with Default Configuration (Simplest)

The easiest way to start a server is with default settings:

```bash
# Start with all defaults (node-id: node-1, listen-addr: 0.0.0.0:8000)
plexspaces start

# Or with custom node ID and address
plexspaces start --node-id my-node --listen-addr 0.0.0.0:8000
```

This automatically creates a default configuration in memory with:
- Node ID: `node-1` (or your custom value)
- Listen address: `0.0.0.0:8000` (or your custom value)
- JWT: Enabled (requires `PLEXSPACES_JWT_SECRET` env var if auth enabled)
- mTLS: Enabled with auto-generation (certificates created in `/app/certs`)

## Option 2: Start with Configuration File

### Step 1: Generate Default Configuration File

```bash
# Generate a default release.yaml file
plexspaces generate-release-config \
  --output release.yaml \
  --release-name "my-cluster" \
  --node-id "my-node" \
  --listen-addr "0.0.0.0:8000"
```

This creates a `release.yaml` file with all default settings that you can customize.

### Step 2: (Optional) Customize the Configuration

Edit `release.yaml` to customize:
- Security settings (JWT, mTLS)
- Node clustering
- Application deployments
- Storage backends
- etc.

### Step 3: Start Server with Configuration File

```bash
# Start with the configuration file
plexspaces start \
  --node-id my-node \
  --listen-addr 0.0.0.0:8000 \
  --release-config release.yaml
```

**Note:** The `--release-config` parameter accepts a file path. You can use:
- Relative paths: `--release-config release.yaml`
- Absolute paths: `--release-config /path/to/release.yaml`
- Different file names: `--release-config my-config.yaml`

## Option 3: Production Setup with mTLS Certificates

### Step 1: Generate mTLS Certificates

```bash
# Generate certificates
plexspaces generate-mtls \
  --output ./certs \
  --ca-common-name "My Company CA" \
  --server-common-name "my-node.example.com" \
  --validity-days 365
```

This creates:
- `./certs/ca.crt` - CA certificate
- `./certs/ca.key` - CA private key
- `./certs/server.crt` - Server certificate
- `./certs/server.key` - Server private key

### Step 2: Set Environment Variables

```bash
export PLEXSPACES_MTLS_CA_CERT="./certs/ca.crt"
export PLEXSPACES_MTLS_SERVER_CERT="./certs/server.crt"
export PLEXSPACES_MTLS_SERVER_KEY="./certs/server.key"
export PLEXSPACES_JWT_SECRET="your-secret-key-here"
```

### Step 3: Start Server

```bash
plexspaces start --node-id my-node
```

## Option 4: Testing Mode (Auth Disabled)

For local development and testing:

```bash
# Disable auth validation
export PLEXSPACES_DISABLE_AUTH=1

# Start server
plexspaces start --node-id test-node
```

## Command Reference

### `plexspaces start`

Start a PlexSpaces node instance.

**Options:**
- `--node-id <ID>`: Node ID (default: `node-1`)
- `--listen-addr <ADDRESS>`: Listen address (default: `0.0.0.0:8000`)
- `--release-config <FILE>`: Path to release configuration file (YAML format)

**Examples:**
```bash
# Default settings
plexspaces start

# Custom node ID and address
plexspaces start --node-id prod-node-1 --listen-addr 0.0.0.0:9000

# With configuration file
plexspaces start --release-config /path/to/release.yaml

# With configuration file (relative path)
plexspaces start --release-config release.yaml
```

### `plexspaces generate-release-config`

Generate a default release configuration file.

**Options:**
- `--output, -o <FILE>`: Output file path (default: `release.yaml`)
- `--release-name <NAME>`: Release name (default: `plexspaces-cluster`)
- `--release-version <VERSION>`: Release version (default: `1.0.0`)
- `--node-id <ID>`: Node ID (default: `node-1`)
- `--listen-addr <ADDRESS>`: Listen address (default: `0.0.0.0:8000`)

**Examples:**
```bash
# Generate with defaults
plexspaces generate-release-config

# Generate with custom values
plexspaces generate-release-config \
  --output my-release.yaml \
  --release-name "production-cluster" \
  --node-id "prod-node-1"
```

### `plexspaces generate-mtls`

Generate mTLS certificates for node-to-node authentication.

**Options:**
- `--output, -o <DIR>`: Output directory (default: `./certs`)
- `--ca-common-name <NAME>`: CA common name (default: `PlexSpaces CA`)
- `--server-common-name <NAME>`: Server common name (default: `PlexSpaces Server`)
- `--validity-days <DAYS>`: Validity in days (default: `90`)

**Examples:**
```bash
# Generate with defaults
plexspaces generate-mtls

# Generate with custom settings
plexspaces generate-mtls \
  --output /path/to/certs \
  --ca-common-name "My Company CA" \
  --server-common-name "my-node.example.com" \
  --validity-days 365
```

## Environment Variables

- `PLEXSPACES_JWT_SECRET`: JWT secret for API authentication
- `PLEXSPACES_MTLS_CA_CERT`: Path to mTLS CA certificate
- `PLEXSPACES_MTLS_SERVER_CERT`: Path to mTLS server certificate
- `PLEXSPACES_MTLS_SERVER_KEY`: Path to mTLS server private key
- `PLEXSPACES_MTLS_CERT_DIR`: Directory for auto-generated certificates (default: `/app/certs`)
- `PLEXSPACES_DISABLE_AUTH`: Disable auth validation (testing only, never in production)
- `RUST_LOG`: Log level (debug, info, warn, error)

## Troubleshooting

### Server won't start

1. Check if the port is already in use:
   ```bash
   lsof -i :8000
   ```

2. Check logs for errors:
   ```bash
   RUST_LOG=debug plexspaces start
   ```

3. Verify configuration file syntax:
   ```bash
   # Try loading the config
   plexspaces generate-release-config --output test.yaml
   # Edit test.yaml and verify it's valid YAML
   ```

### Certificate errors

1. Ensure certificates exist:
   ```bash
   ls -la ./certs/
   ```

2. Regenerate certificates if needed:
   ```bash
   plexspaces generate-mtls --output ./certs
   ```

3. Check file permissions:
   ```bash
   chmod 600 ./certs/*.key  # Private keys should be readable only by owner
   ```

### Authentication errors

1. For testing, disable auth:
   ```bash
   export PLEXSPACES_DISABLE_AUTH=1
   ```

2. For production, ensure JWT secret is set:
   ```bash
   export PLEXSPACES_JWT_SECRET="your-secret-here"
   ```

## Next Steps

- See [CLI Reference](cli.md) for all available commands
- See [Installation Guide](installation.md) for deployment options
- See [Security Guide](security.md) for security best practices
