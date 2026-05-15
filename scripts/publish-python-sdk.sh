#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Publish the PlexSpaces Python SDK to PyPI.
#
# Usage:
#   ./scripts/publish-python-sdk.sh [--test]
#
# Flags:
#   --test   Publish to TestPyPI instead of production PyPI
#
# twine upload always runs with --verbose so the full HTTP response is visible.
#
# Prerequisites:
#   pip install build twine
#   PyPI API token set via TWINE_PASSWORD env var or ~/.pypirc

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SDK_DIR="$REPO_ROOT/sdks/python"
TEST_MODE=0

for arg in "$@"; do
    case "$arg" in
        --test) TEST_MODE=1 ;;
        *) echo -e "${RED}Unknown argument: $arg${NC}"; exit 1 ;;
    esac
done

echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Publishing PlexSpaces Python SDK                                ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""

if [ "$TEST_MODE" = "1" ]; then
    echo -e "${YELLOW}Mode: TestPyPI${NC}"
    REPOSITORY_URL="https://test.pypi.org/legacy/"
    REPOSITORY_FLAG="--repository-url $REPOSITORY_URL"
else
    echo "Mode: Production PyPI"
    REPOSITORY_FLAG=""
fi

# Check tools
for tool in python3 pip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo -e "${RED}ERROR: $tool not found${NC}"
        exit 1
    fi
done

echo "[1/3] Installing build tools..."
pip install --quiet build twine
echo -e "${GREEN}  ✅ build + twine ready${NC}"

echo "[2/3] Building distribution packages..."
cd "$SDK_DIR"
rm -rf dist/ build/ *.egg-info
python3 -m build
echo -e "${GREEN}  ✅ Packages built:${NC}"
ls -lh dist/

echo "[3/3] Uploading to PyPI..."
if [ "$TEST_MODE" = "1" ]; then
    twine upload --verbose --repository-url "$REPOSITORY_URL" dist/*
else
    twine upload --verbose dist/*
fi

echo ""
echo -e "${GREEN}✅ Python SDK published successfully.${NC}"
echo ""
echo "Install with:"
VERSION=$(python3 -c "import tomllib; d=tomllib.load(open('pyproject.toml','rb')); print(d['project']['version'])" 2>/dev/null || python3 -c "import tomli; d=tomli.load(open('pyproject.toml','rb')); print(d['project']['version'])" 2>/dev/null || grep '^version' pyproject.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
echo "  pip install plexspaces==${VERSION}"
