#!/usr/bin/env bash
# Generate a test JWT (ES256) for local development/testing.
# Usage: eval $(./scripts/gen-test-jwt.sh)
#   Then use: curl -H "Authorization: Bearer $PLEXSPACES_TEST_TOKEN" ...
#
# Requires: python3 with cryptography module (pip install cryptography)
# Uses: PLEXSPACES_JWT_PRIVATE_KEY_FILE or PLEXSPACES_JWT_PRIVATE_KEY
#
# Environment:
#   PLEXSPACES_JWT_PRIVATE_KEY_FILE - Path to ES256 PEM private key file
#   PLEXSPACES_JWT_PRIVATE_KEY      - Inline ES256 PEM private key
#   PLEXSPACES_TEST_TENANT          - tenant_id claim (default: "test-tenant")
#   PLEXSPACES_TEST_USER            - sub claim (default: "test-user")
#   PLEXSPACES_TEST_ADMIN           - is_admin claim (default: "true")

set -euo pipefail

TENANT="${PLEXSPACES_TEST_TENANT:-test-tenant}"
USER="${PLEXSPACES_TEST_USER:-test-user}"
ADMIN="${PLEXSPACES_TEST_ADMIN:-true}"

PRIVATE_KEY_FILE="${PLEXSPACES_JWT_PRIVATE_KEY_FILE:-}"
PRIVATE_KEY="${PLEXSPACES_JWT_PRIVATE_KEY:-}"

# Try to read from server meta file if nothing is set
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEN_JWT_REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if [ -z "$PRIVATE_KEY_FILE" ] && [ -z "$PRIVATE_KEY" ]; then
  for META_DIR in "." "$GEN_JWT_REPO_ROOT"; do
    for META in "$META_DIR"/plexspaces-node-*.meta; do
      if [ -f "$META" ]; then
        META_KEY_FILE=$(grep '^jwt_private_key_file=' "$META" 2>/dev/null | cut -d= -f2-)
        if [ -n "$META_KEY_FILE" ] && [ -f "$META_KEY_FILE" ]; then
          PRIVATE_KEY_FILE="$META_KEY_FILE"
          break 2
        elif [ -n "$META_KEY_FILE" ] && [ -f "$GEN_JWT_REPO_ROOT/$META_KEY_FILE" ]; then
          PRIVATE_KEY_FILE="$GEN_JWT_REPO_ROOT/$META_KEY_FILE"
          break 2
        fi
      fi
    done
  done
fi

# Activate venv and ensure cryptography is installed
if [ -f "$HOME/venv/bin/activate" ]; then
  source "$HOME/venv/bin/activate"
fi
if ! python3 -c "import cryptography" 2>/dev/null; then
  echo "Installing python3 cryptography module..." >&2
  pip3 install -q cryptography 2>/dev/null || {
    echo "ERROR: Failed to install cryptography module. Run: pip3 install cryptography" >&2
    exit 1
  }
fi

KEY_SOURCE=""
if [ -n "$PRIVATE_KEY_FILE" ] && [ -f "$PRIVATE_KEY_FILE" ]; then
  KEY_SOURCE="$PRIVATE_KEY_FILE"
elif [ -n "$PRIVATE_KEY" ]; then
  KEY_SOURCE=$(umask 077; mktemp)
  echo "$PRIVATE_KEY" > "$KEY_SOURCE"
  trap "rm -f $KEY_SOURCE" EXIT
else
  echo "ERROR: No ES256 private key available." >&2
  echo "Set PLEXSPACES_JWT_PRIVATE_KEY_FILE=./certs/jwt-es256.pem or PLEXSPACES_JWT_PRIVATE_KEY" >&2
  echo "The server auto-generates this key on first start via scripts/server.sh" >&2
  exit 1
fi

TOKEN=$(KEY_FILE="$KEY_SOURCE" TENANT="$TENANT" USER="$USER" ADMIN="$ADMIN" python3 - <<'PY'
import base64, json, os, time

def b64url(data):
    if isinstance(data, str):
        data = data.encode()
    return base64.urlsafe_b64encode(data).rstrip(b'=').decode()

try:
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec, utils
    from cryptography.hazmat.backends import default_backend
except ImportError:
    print("ERROR: python3 'cryptography' module required for ES256.", file=__import__('sys').stderr)
    print("Install: pip3 install cryptography", file=__import__('sys').stderr)
    raise SystemExit(1)

key_file = os.environ["KEY_FILE"]
tenant = os.environ["TENANT"]
user = os.environ["USER"]
admin = os.environ.get("ADMIN", "true").lower() == "true"

with open(key_file, "rb") as f:
    private_key = serialization.load_pem_private_key(f.read(), password=None, backend=default_backend())

header = b64url(json.dumps({"alg": "ES256", "typ": "JWT"}, separators=(',', ':')))
now = int(time.time())
payload = b64url(json.dumps({
    "sub": user,
    "tenant_id": tenant,
    "is_admin": admin,
    "roles": ["admin", "developer"],
    "groups": [],
    "iat": now,
    "exp": now + 86400,
    "iss": "plexspaces",
    "aud": "plexspaces-api",
}, separators=(',', ':')))

signing_input = f"{header}.{payload}".encode()
der_sig = private_key.sign(signing_input, ec.ECDSA(hashes.SHA256()))

# Convert DER signature to raw r||s (64 bytes for P-256)
r, s = utils.decode_dss_signature(der_sig)
raw_sig = r.to_bytes(32, 'big') + s.to_bytes(32, 'big')
signature = base64.urlsafe_b64encode(raw_sig).rstrip(b'=').decode()

print(f"{header}.{payload}.{signature}")
PY
)

echo "export PLEXSPACES_TEST_TOKEN=\"$TOKEN\""
echo "# Algorithm: ES256" >&2
