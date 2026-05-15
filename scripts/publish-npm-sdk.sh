#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Publish the PlexSpaces TypeScript/JavaScript SDK to npm.
#
# Usage:
#   ./scripts/publish-npm-sdk.sh [--dry-run]
#
# Flags:
#   --dry-run   Run npm publish --dry-run (no actual upload)
#
# Prerequisites:
#   npm login  (or set NPM_TOKEN env var for CI)

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SDK_DIR="$REPO_ROOT/sdks/typescript"
DRY_RUN=0

for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN=1 ;;
        *) echo -e "${RED}Unknown argument: $arg${NC}"; exit 1 ;;
    esac
done

echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Publishing PlexSpaces TypeScript SDK to npm                     ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""

if [ "$DRY_RUN" = "1" ]; then
    echo -e "${YELLOW}Mode: dry-run (no actual publish)${NC}"
fi

if ! command -v npm >/dev/null 2>&1; then
    echo -e "${RED}ERROR: npm not found${NC}"
    exit 1
fi

echo "[1/3] Building SDK..."
cd "$SDK_DIR"
npm install --no-audit --no-fund
npm run build
echo -e "${GREEN}  ✅ SDK built${NC}"

echo "[2/3] Verifying package..."
npm pack --dry-run
echo -e "${GREEN}  ✅ Package verified${NC}"

echo "[3/3] Publishing..."
if [ "$DRY_RUN" = "1" ]; then
    npm publish --access public --dry-run
else
    npm publish --access public
fi

echo ""
echo -e "${GREEN}✅ TypeScript SDK published successfully.${NC}"
echo ""
VERSION=$(node -p "require('./package.json').version")
echo "Install with:"
echo "  npm install @plexspaces/sdk@${VERSION}"
