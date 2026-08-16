#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail
HTTP_PORT="${1:-8091}"
APP_ID="perf-python"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" || true
echo "Undeployed $APP_ID"
