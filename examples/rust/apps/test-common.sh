#!/usr/bin/env bash
# Shared helpers for examples/rust/apps/*/test.sh (deploy + application metrics).
# shellcheck shell=bash
#
# Source from a per-app test script:
#   SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
#   _RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
#   # shellcheck source=/dev/null
#   source "${_RUST_APPS_ROOT}/test-common.sh"
#
# Application list: use gRPC ApplicationService.ListApplications (see application.proto).
# scripts/server.sh: gRPC and HTTP share a single port — grpc_port equals http_port.

# Fetch JSON shaped like ListApplicationsResponse: { "applications": [ ... ApplicationInfo ... ] }.
# Uses the PlexSpaces CLI (`plexspaces list --json`), which calls gRPC ListApplications — not HTTP.
#
# Arg: base URL with no trailing slash, e.g. http://localhost:8091 (gRPC and HTTP share this port).
# Optional: set PLEXSPACES to the CLI binary path (default: repo target/debug/plexspaces).
fetch_applications_list_json() {
  local base="${1:?base URL required}"
  local host port grpc_port repo_root cli
  if [[ "$base" =~ ^https?://([^:/]+):([0-9]+) ]]; then
    host="${BASH_REMATCH[1]}"
    port="${BASH_REMATCH[2]}"
    grpc_port=$port
  else
    echo '{"applications":[]}'
    return 0
  fi
  repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
  cli="${PLEXSPACES:-$repo_root/target/debug/plexspaces}"
  if [ ! -f "$cli" ]; then
    echo "fetch_applications_list_json: CLI not found at $cli (set PLEXSPACES or cargo build -p plexspaces-cli)" >&2
    echo '{"applications":[]}'
    return 0
  fi
  "$cli" list --node "${host}:${grpc_port}" --json 2>/dev/null || echo '{"applications":[]}'
}
