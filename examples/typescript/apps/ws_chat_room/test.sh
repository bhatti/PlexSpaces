#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Integration test for ws_chat_room.
#
# Two Node.js WsThinClient sessions (alice, bob) join the same room,
# alice sends a message, bob asserts receipt.
#
# Usage:
#   bash test.sh [port]
#   KEEP_DEPLOYED=1 bash test.sh   # skip undeploy so browser UI stays accessible
#
# Environment overrides:
#   WS_PORT, WS_URL, HTTP_URL, LEADER_NODE_ID, TIMEOUT (seconds)
#   KEEP_DEPLOYED=0   undeploy at the end (default is to keep it deployed)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SDK_DIR="$REPO_ROOT/sdks/typescript/dist"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/ws_chat_room"
OUTPUT_WASM="$TARGET_DIR/chat_server_actor.wasm"

WS_PORT="${1:-${WS_PORT:-8091}}"
WS_URL="${WS_URL:-ws://localhost:${WS_PORT}/ws}"
HTTP_URL="${HTTP_URL:-http://localhost:${WS_PORT}}"
APP_ID="ts-ws-chat-room"
LEADER_NODE_ID="${LEADER_NODE_ID:-test-node-${WS_PORT}}"
TIMEOUT="${TIMEOUT:-60}"

log() { echo "[test.sh] $*" >&2; }
fail() { echo "[FAIL] $*" >&2; exit 1; }

# ─── 0. Build ─────────────────────────────────────────────────────────────────
log "Building ws_chat_room…"
cd "$SCRIPT_DIR"
bash build.sh
[ -f "$OUTPUT_WASM" ] || fail "WASM not built: $OUTPUT_WASM"
[ -f "$SDK_DIR/ws_thin_client.js" ] || fail "TypeScript SDK not built. Run: cd $REPO_ROOT/sdks/typescript && npm run build"

# ─── 1. JWT auth (auto-generate if server has auth enabled) ───────────────────
export AUTH_HEADER="${AUTH_HEADER:-}"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi
WS_TOKEN="${PLEXSPACES_TEST_TOKEN:-}"

# ─── 2. Check node is reachable ───────────────────────────────────────────────
log "Checking node at ${HTTP_URL}…"
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "${OUTPUT_WASM}" "${SCRIPT_DIR}/app-config.toml" >/dev/null
(cd "${SCRIPT_DIR}" && zip "$APP_ZIP" static/ static/* 2>/dev/null || true)
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "${HTTP_URL}/health" 2>/dev/null || echo "000")
[ "$HTTP_CODE" = "200" ] || fail "Node not reachable (HTTP $HTTP_CODE). Start with: ./scripts/server.sh"

# ─── 3. Undeploy any previous run, then deploy ────────────────────────────────
bash "$SCRIPT_DIR/undeploy.sh" "$WS_PORT" 2>/dev/null || true
sleep 1

log "Deploying ${APP_ID}…"
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "${HTTP_URL}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=${APP_ID}" \
    -F "name=${APP_ID}" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  log "Deploy attempt $_attempt failed (HTTP $HTTP_CODE), retrying…"
  sleep 3
done
[ "$_deployed" -eq 1 ] || fail "Deploy failed after 3 attempts: $RESPONSE"
log "Deployed OK"

# ─── 4. Integration test ──────────────────────────────────────────────────────
log "Running Node.js integration test…"

node --input-type=module <<NODEOF
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

// Import only the thin-client modules — avoids plexspaces: WIT host imports
// that exist in actor.js/host.js and cannot be resolved outside WASM.
const SDK_DIR = "${SDK_DIR}";
const { WsThinClient } = await import(pathToFileURL(SDK_DIR + "/ws_thin_client.js").href);
const { ActorID } = await import(pathToFileURL(SDK_DIR + "/actor_id.js").href);

const WS_URL         = "${WS_URL}";
const WS_TOKEN       = "${WS_TOKEN}";
const LEADER_NODE_ID = "${LEADER_NODE_ID}";
const APP_NS         = "${APP_ID}";
const ROOM           = "test-lobby-" + WsThinClient.newUlid().slice(-6);
const TIMEOUT_MS     = ${TIMEOUT} * 1000;

function buildChatRoomId(room, leaderNodeId) {
  return new ActorID(room, "ChatRoomActor", APP_NS, leaderNodeId).toString();
}

async function connectClient(name) {
  const opts = { wsUrl: WS_URL, nodeId: WsThinClient.newUlid(), namespace: APP_NS };
  if (WS_TOKEN) opts.jwtToken = WS_TOKEN;
  const c = new WsThinClient(opts);
  const nodeId = await c.connect();
  const actorId = c.localActorId(name, "ChatClient", APP_NS);
  return { client: c, nodeId, actorId };
}

async function runTest() {
  const chatRoomId = buildChatRoomId(ROOM, LEADER_NODE_ID);
  const { client: alice, actorId: aliceActor } = await connectClient("alice");
  const { client: bob,   actorId: bobActor }   = await connectClient("bob");

  let msgReceived = null;
  const messagePromise = new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("Timeout waiting for chat_message")), TIMEOUT_MS);
    bob.onMessage((_from, msgType, payload) => {
      if (msgType === "chat_message") {
        clearTimeout(t);
        msgReceived = payload;
        resolve(payload);
      }
    });
  });

  await alice.ask(chatRoomId, "join", { actor_id: aliceActor, username: "alice" }, 10_000);
  await bob.ask(chatRoomId,   "join", { actor_id: bobActor,   username: "bob"   }, 10_000);

  const testText = "hello from alice " + Date.now();
  await alice.tell(chatRoomId, "send", { sender_actor_id: aliceActor, text: testText, _t: Date.now() });

  const received = await messagePromise;
  if (received.text !== testText) {
    throw new Error("Expected text '" + testText + "', got '" + received.text + "'");
  }

  await alice.ask(chatRoomId, "leave", { actor_id: aliceActor }, 5_000);
  await bob.ask(chatRoomId,   "leave", { actor_id: bobActor   }, 5_000);
  await alice.disconnect();
  await bob.disconnect();

  console.log("[test] PASS: alice → ChatRoomActor PG broadcast → bob message delivery confirmed");
}

await runTest().catch(err => { console.error("[test] FAIL:", err.message); process.exit(1); });
NODEOF

# ─── 5. Undeploy (skipped if KEEP_DEPLOYED=1) ─────────────────────────────────
if [ "${KEEP_DEPLOYED:-1}" = "1" ]; then
  log "KEEP_DEPLOYED=1 — skipping undeploy"
  log "Browser UI available at: ${HTTP_URL}/apps/${APP_ID}/"
else
  log "Undeploying ${APP_ID}…"
  bash "$SCRIPT_DIR/undeploy.sh" "$WS_PORT"
fi

log "ALL TESTS PASSED"
