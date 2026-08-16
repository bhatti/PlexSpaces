#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Generate a test JWT for the ws_chat_room example.
# Embeds username (sub), tenant_id, and namespace into the token so the
# chat UI auto-fills from it on paste.
#
# Usage:
#   ./gen-test-jwt.sh [username] [tenant]
#
#   # Or eval to export:
#   eval $(./gen-test-jwt.sh alice default)
#   echo $PLEXSPACES_TEST_TOKEN
#
# Requires: python3 with 'cryptography' (pip install cryptography)
# Reads key from: PLEXSPACES_JWT_PRIVATE_KEY_FILE, or auto-discovers
#   plexspaces-node-*.meta in the repo root.
#
# Output: prints  export PLEXSPACES_TEST_TOKEN="<jwt>"
#         so you can copy the token directly into the chat UI JWT field.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

USERNAME="${1:-${PLEXSPACES_TEST_USER:-alice}}"
TENANT="${2:-${PLEXSPACES_TEST_TENANT:-default}}"
NAMESPACE="${PLEXSPACES_CHAT_NAMESPACE:-ts-ws-chat-room}"
ADMIN="${PLEXSPACES_TEST_ADMIN:-true}"

# ── Locate private key ────────────────────────────────────────────────────────

PRIVATE_KEY_FILE="${PLEXSPACES_JWT_PRIVATE_KEY_FILE:-}"

if [ -z "$PRIVATE_KEY_FILE" ]; then
  for META in "$REPO_ROOT"/plexspaces-node-*.meta; do
    if [ -f "$META" ]; then
      KEY_FROM_META=$(grep '^jwt_private_key_file=' "$META" 2>/dev/null | cut -d= -f2- || true)
      if [ -n "$KEY_FROM_META" ]; then
        if [ -f "$KEY_FROM_META" ]; then
          PRIVATE_KEY_FILE="$KEY_FROM_META"
        elif [ -f "$REPO_ROOT/$KEY_FROM_META" ]; then
          PRIVATE_KEY_FILE="$REPO_ROOT/$KEY_FROM_META"
        fi
        break
      fi
    fi
  done
fi

if [ -z "$PRIVATE_KEY_FILE" ]; then
  echo "ERROR: No JWT private key found." >&2
  echo "  Set PLEXSPACES_JWT_PRIVATE_KEY_FILE=<path to ES256 PEM>" >&2
  echo "  Or start the server with: ./scripts/server.sh (auto-generates a key)" >&2
  exit 1
fi

# ── Activate venv and ensure cryptography ────────────────────────────────────

if [ -f "$HOME/venv/bin/activate" ]; then
  # shellcheck disable=SC1091
  source "$HOME/venv/bin/activate"
fi
if ! python3 -c "import cryptography" 2>/dev/null; then
  echo "Installing python3 cryptography…" >&2
  pip3 install -q cryptography >&2
fi

# ── Generate token ────────────────────────────────────────────────────────────

TOKEN=$(
  KEY_FILE="$PRIVATE_KEY_FILE" \
  USERNAME="$USERNAME" \
  TENANT="$TENANT" \
  NAMESPACE="$NAMESPACE" \
  ADMIN="$ADMIN" \
  python3 - <<'PY'
import base64, json, os, time

def b64url(data):
    if isinstance(data, str): data = data.encode()
    return base64.urlsafe_b64encode(data).rstrip(b'=').decode()

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, utils
from cryptography.hazmat.backends import default_backend

key_file  = os.environ["KEY_FILE"]
username  = os.environ["USERNAME"]
tenant    = os.environ["TENANT"]
namespace = os.environ["NAMESPACE"]
admin     = os.environ.get("ADMIN", "true").lower() == "true"

with open(key_file, "rb") as f:
    private_key = serialization.load_pem_private_key(f.read(), password=None, backend=default_backend())

header  = b64url(json.dumps({"alg": "ES256", "typ": "JWT"}, separators=(',', ':')))
now     = int(time.time())
payload = b64url(json.dumps({
    "sub":        username,
    "tenant_id":  tenant,
    "namespace":  namespace,
    "is_admin":   admin,
    "roles":      ["admin", "developer"],
    "groups":     [],
    "iat":        now,
    "exp":        now + 86400,
    "iss":        "plexspaces",
    "aud":        "plexspaces-api",
}, separators=(',', ':')))

signing_input = f"{header}.{payload}".encode()
der_sig = private_key.sign(signing_input, ec.ECDSA(hashes.SHA256()))
r, s = utils.decode_dss_signature(der_sig)
raw_sig = r.to_bytes(32, 'big') + s.to_bytes(32, 'big')
signature = base64.urlsafe_b64encode(raw_sig).rstrip(b'=').decode()

print(f"{header}.{payload}.{signature}")
PY
)

echo "export PLEXSPACES_TEST_TOKEN=\"$TOKEN\""
echo "# Username: $USERNAME | Tenant: $TENANT | Namespace: $NAMESPACE" >&2
echo "# Paste the token value into the JWT Token field in the chat UI." >&2
