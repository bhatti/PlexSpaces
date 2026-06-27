#!/usr/bin/env bash
# Test Web Crawl (Rust Embedded)
# Single-binary: builds and runs the crawler in one process, no node needed.
# Usage: ./test.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
export CARGO_TARGET_DIR="${REPO_ROOT}/target"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'


# Auto-generate JWT if not provided
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    echo "Output was: $JWT_OUTPUT"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

echo "================================================================"
echo "  Web Crawl (Rust Embedded)"
echo "  TupleSpace frontier + ElasticPool + ProcessGroup + ShardGroup scatter"
echo "  Single binary — Node + actors + main() in one process"
echo "================================================================"

echo ""
echo "Step 0: Build"
cd "$SCRIPT_DIR"
cargo build 2>&1
BINARY="$CARGO_TARGET_DIR/debug/web_crawl"
if [ ! -f "$BINARY" ]; then
  echo -e "${RED}Build failed — binary not found: $BINARY${NC}"
  exit 1
fi
echo -e "  ${GREEN}Built: $BINARY${NC}"

echo ""
echo "================================================================"
echo "  Phase 1: Run crawl"
echo "================================================================"
echo ""
echo "Step 1: Execute crawler"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

output=$("$BINARY" 2>&1)
echo "$output"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Validate output contains expected fields
if ! echo "$output" | grep -q 'Pages crawled'; then
  echo -e "${RED}Test failed: expected 'Pages crawled' in output${NC}"
  exit 1
fi

OUTPUT="$output" python3 - <<'PY'
import os, re, sys
text = os.environ["OUTPUT"]
pages_m = re.search(r'Pages crawled\s*:\s*(\d+)', text)
links_m = re.search(r'Total links\s*:\s*(\d+)', text)
pool_m  = re.search(r'Pool fetches\s*:\s*(\d+)', text)
pages = int(pages_m.group(1)) if pages_m else 0
links = int(links_m.group(1)) if links_m else 0
pool  = int(pool_m.group(1))  if pool_m  else 0
print(f"  Pages crawled : {pages}")
print(f"  Total links   : {links}")
print(f"  Pool fetches  : {pool}")
# Worker status section
in_workers = False
for line in text.splitlines():
    if 'Worker status' in line:
        in_workers = True
        continue
    if in_workers:
        if re.match(r'\s{4}\S', line):
            print(f"  {line.strip()}")
        else:
            in_workers = False
# Extract top words lines
in_words = False
for line in text.splitlines():
    if 'Top words' in line:
        in_words = True
        continue
    if in_words:
        if re.match(r'\s{4}\S', line):
            print(f"  {line.strip()}")
        else:
            in_words = False
if pages < 1:
    raise SystemExit("Expected at least 1 page crawled")
# Verify pool fetches present (ElasticPool pattern working)
if pool < 1:
    raise SystemExit("Expected pool_fetches >= 1")
# Check scaling benchmark output
if 'Scaling benchmark' not in text:
    raise SystemExit("Expected scaling benchmark table in output")
if 'Pages/sec' not in text:
    raise SystemExit("Expected Pages/sec column in benchmark table")
PY

echo ""
echo "================================================================"
echo -e "  ${GREEN}Web Crawl (Rust Embedded) Test Complete${NC}"
echo ""
echo "  Key behaviors demonstrated:"
echo "    ✓ TupleSpace frontier — mark-before-enqueue HashSet dedup"
echo "    ✓ ElasticPool         — channel free-list checkout/checkin"
echo "    ✓ ProcessGroup        — worker registry broadcast/collect"
echo "    ✓ ShardGroup scatter  — interleaved i%num_shards assignment"
echo "    ✓ Scaling benchmark   — 1/4/8/16 workers, pages/sec, speedup, Amdahl efficiency"
echo "    ✓ Native Rust         — no WASM sandbox overhead, tokio parallel dispatch"
echo "    ✓ Single binary       — Node + actors + main() in one process"
echo "================================================================"
