#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Orchestrate all SDK publishes (Python + npm).
# Docker is published separately via scripts/publish-docker.sh.
# Git tagging is done separately via scripts/tag-release.sh.
#
# Usage:
#   ./scripts/publish-sdks.sh [--python] [--npm] [--all] [--dry-run]
#
# Flags:
#   --python   Publish Python SDK to PyPI
#   --npm      Publish TypeScript SDK to npm
#   --all      Publish all SDKs (default if no flag given)
#   --dry-run  Pass --dry-run to underlying scripts where supported

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS="$REPO_ROOT/scripts"

DO_PYTHON=0
DO_NPM=0
DRY_RUN_FLAG=""

for arg in "$@"; do
    case "$arg" in
        --python) DO_PYTHON=1 ;;
        --npm)    DO_NPM=1 ;;
        --all)    DO_PYTHON=1; DO_NPM=1 ;;
        --dry-run) DRY_RUN_FLAG="--dry-run" ;;
        *) echo -e "${RED}Unknown argument: $arg${NC}"; exit 1 ;;
    esac
done

# Default: all
if [ "$DO_PYTHON" = "0" ] && [ "$DO_NPM" = "0" ]; then
    DO_PYTHON=1
    DO_NPM=1
fi

echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  PlexSpaces SDK Publish Orchestrator                             ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""

if [ "$DO_PYTHON" = "1" ]; then
    echo "── Python SDK ────────────────────────────────────────────────────"
    bash "$SCRIPTS/publish-python-sdk.sh" $DRY_RUN_FLAG
    echo ""
fi

if [ "$DO_NPM" = "1" ]; then
    echo "── TypeScript SDK ────────────────────────────────────────────────"
    bash "$SCRIPTS/publish-npm-sdk.sh" $DRY_RUN_FLAG
    echo ""
fi

echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Rust SDK (via git tag)                                          ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "The Rust SDK is imported directly from git. External projects use:"
echo ""
echo -e "${YELLOW}  # In Cargo.toml:${NC}"
echo -e "${YELLOW}  plexspaces-sdk = { git = \"https://github.com/plexobject/plexspaces\", tag = \"v0.1.0\" }${NC}"
echo ""
echo "To create a new tag, run:"
echo -e "${YELLOW}  ./scripts/tag-release.sh v0.1.0${NC}"
echo ""

echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  Go SDK (via Go module proxy)                                    ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "The Go SDK is served via the Go module proxy from the repo tag."
echo "tag-release.sh creates the required sdks/go/v<VERSION> tag."
echo ""
echo "External projects use:"
echo -e "${YELLOW}  require github.com/plexobject/plexspaces/sdks/go v0.1.0${NC}"
echo "(no replace directive needed once the tag is pushed)"
echo ""

echo -e "${GREEN}✅ SDK publish orchestration complete.${NC}"
