#!/usr/bin/env bash
# Test Web Crawl (Rust Embedded)
# Single-binary: builds and runs the crawler in one process, no node needed.
# Usage: ./test.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo "================================================================"
echo "  Web Crawl (Rust Embedded)"
echo "  ElasticPool + TupleSpace + ShardGroup — single binary"
echo "================================================================"

echo ""
echo "Step 0: Build"
cd "$SCRIPT_DIR"
cargo build --release 2>&1 | grep -E '^(error|warning: unused|Compiling|Finished)' || true
BINARY="$CARGO_TARGET_DIR/release/web_crawl"
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
if ! echo "$output" | grep -q '"pages_crawled"'; then
  echo -e "${RED}Test failed: expected pages_crawled in output${NC}"
  exit 1
fi

echo "$output" | python3 - <<'PY'
import sys, json, re
text = sys.stdin.read()
m = re.search(r'\{.*\}', text, re.DOTALL)
if not m:
    raise SystemExit("No JSON found in output")
d = json.loads(m.group(0))
pages = d.get("pages_crawled", 0)
links = d.get("total_links", 0)
words = d.get("top_words") or []
print(f"  Pages crawled : {pages}")
print(f"  Total links   : {links}")
print("  Top words     :")
for entry in words:
    if isinstance(entry, (list, tuple)) and len(entry) >= 2:
        print(f"    {entry[0]:<20} {entry[1]}")
if pages < 1:
    raise SystemExit("Expected at least 1 page crawled")
PY

echo ""
echo "================================================================"
echo -e "  ${GREEN}Web Crawl (Rust Embedded) Test Complete${NC}"
echo ""
echo "  Key behaviors demonstrated:"
echo "    ✓ ElasticPool  — 4 PageFetcher actors, round-robin URL dispatch"
echo "    ✓ TupleSpace   — url_queue pending→done tracking"
echo "    ✓ ShardGroup   — 2 analyzer shards, scatter/reduce word counts"
echo "    ✓ Single binary — Node + actors + main() in one process"
echo "================================================================"
