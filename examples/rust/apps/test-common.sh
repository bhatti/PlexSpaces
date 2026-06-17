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

# Count active (status=2) nodes visible from a given node's registry.
# Args: host port [auth_header]
# Prints: integer count
list_registered_node_count() {
  local host="$1" port="$2"
  local raw
  raw="$(curl -s --connect-timeout 5 --max-time 15 "http://${host}:${port}/api/v1/nodes?page_size=100" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} 2>/dev/null || true)"
  RAW_RESPONSE="$raw" python3 - <<'PY'
import json, os
raw = os.environ.get("RAW_RESPONSE", "").strip()
if not raw:
    print(0)
    raise SystemExit(0)
try:
    payload = json.loads(raw)
except Exception:
    print(0)
    raise SystemExit(0)
nodes = payload.get("nodes", [])
if not isinstance(nodes, list):
    print(0)
    raise SystemExit(0)
active = [n for n in nodes if n.get("status") == 2]
print(len(active))
PY
}

# Poll all nodes in NODE_LIST until each sees at least $expected active peers.
# Exits non-zero with diagnostic dump if convergence doesn't happen in time.
# Args: expected_count [max_attempts=5] [sleep_seconds=2]
# Requires: NODE_LIST array, AUTH_HEADER (optional), YELLOW/RED/GREEN/NC color vars.
wait_for_registry_membership() {
  local expected="$1"
  local attempts="${2:-5}"
  local sleep_s="${3:-2}"

  for attempt in $(seq 1 "$attempts"); do
    local ready=true
    for node in "${NODE_LIST[@]}"; do
      local host="${node%%:*}"
      local port="${node##*:}"
      local count
      count="$(list_registered_node_count "$host" "$port")"
      if [[ "$count" -lt "$expected" ]]; then
        ready=false
        echo -e "  ${YELLOW:-}Registry on ${host}:${port} sees ${count}/${expected} nodes (attempt ${attempt}/${attempts})...${NC:-}"
        break
      fi
    done
    if [[ "$ready" == "true" ]]; then
      return 0
    fi
    sleep "$sleep_s"
  done

  echo -e "${RED:-}Node registry did not converge to ${expected} nodes${NC:-}"
  for node in "${NODE_LIST[@]}"; do
    local host="${node%%:*}"
    local port="${node##*:}"
    echo "  Registered nodes from http://${host}:${port}/api/v1/nodes"
    curl -s --connect-timeout 5 --max-time 30 "http://${host}:${port}/api/v1/nodes?page_size=100" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} | python3 -m json.tool 2>/dev/null | sed 's/^/    /' || echo "    (request failed)"
  done
  return 1
}
