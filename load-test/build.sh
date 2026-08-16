#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build load-test artifacts — only rebuilds what has changed.
#
# USAGE:
#   bash load-test/build.sh [TARGET...]
#
# TARGETS (default: all):
#   server        Main PlexSpaces server (release)
#   embedded      Rust embedded perf_actor (release)
#   python        Python WASM perf actor
#   go            Go (TinyGo) WASM perf actor
#   typescript    TypeScript WASM perf actor
#   rust-wasm     Rust WASM perf actor
#   all           Everything above (default)
#
# EXAMPLES:
#   bash load-test/build.sh                  # rebuild everything that's stale
#   bash load-test/build.sh server embedded  # only the two native binaries
#   bash load-test/build.sh python go        # only WASM actors

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
APPS_DIR="$SCRIPT_DIR/apps"

# ── Helpers ───────────────────────────────────────────────────────────────────
ok()   { echo "  ✓  $*"; }
skip() { echo "  –  $* (up to date)"; }
die()  { echo "  ✗  $*"; exit 1; }
section() { echo ""; echo "── $* ──────────────────────────────────────────────"; }

# Returns 0 (stale) if $1 binary is missing or any $2..N source files are newer.
is_stale() {
  local bin="$1"; shift
  [ ! -f "$bin" ] && return 0
  for src in "$@"; do
    [ -f "$src" ] && [ "$src" -nt "$bin" ] && return 0
  done
  return 1
}

# ── Determine what to build ───────────────────────────────────────────────────
BUILD_SERVER=0; BUILD_EMBEDDED=0
BUILD_PYTHON=0; BUILD_GO=0; BUILD_TS=0; BUILD_RUST_WASM=0

if [ $# -eq 0 ]; then
  BUILD_SERVER=1; BUILD_EMBEDDED=1
  BUILD_PYTHON=1; BUILD_GO=1; BUILD_TS=1; BUILD_RUST_WASM=1
else
  for arg in "$@"; do
    case "$arg" in
      server)     BUILD_SERVER=1 ;;
      embedded)   BUILD_EMBEDDED=1 ;;
      python)     BUILD_PYTHON=1 ;;
      go)         BUILD_GO=1 ;;
      typescript) BUILD_TS=1 ;;
      rust-wasm)  BUILD_RUST_WASM=1 ;;
      all)
        BUILD_SERVER=1; BUILD_EMBEDDED=1
        BUILD_PYTHON=1; BUILD_GO=1; BUILD_TS=1; BUILD_RUST_WASM=1
        ;;
      *) echo "Unknown target: $arg"; echo "Valid: server embedded python go typescript rust-wasm all"; exit 1 ;;
    esac
  done
fi

echo "PlexSpaces Load Test Builder"
echo "Repo: $REPO_ROOT"

# ── Main server (release) ─────────────────────────────────────────────────────
if [ "$BUILD_SERVER" -eq 1 ]; then
  section "Main server (plexspaces, release)"
  # Always invoke cargo — it handles incremental compilation and is the only
  # tool that correctly tracks which crate sources have changed.
  echo "  Building plexspaces (release)..."
  cargo build --release -p plexspaces-cli || die "plexspaces build failed"
  ok "plexspaces → $REPO_ROOT/target/release/plexspaces"
fi

# ── Embedded actor (release) ──────────────────────────────────────────────────
if [ "$BUILD_EMBEDDED" -eq 1 ]; then
  section "Embedded actor (perf_actor, release)"
  SRC_DIR="$APPS_DIR/rust/embedded/perf_actor"
  # Always invoke cargo — perf_actor links framework crates; only cargo knows
  # whether a transitive dependency changed.
  echo "  Building perf_actor (release)..."
  CARGO_TARGET_DIR="$REPO_ROOT/target" \
    cargo build --release --manifest-path "$SRC_DIR/Cargo.toml" || die "perf_actor build failed"
  ok "perf_actor → $REPO_ROOT/target/release/perf_actor"
fi

# ── Python WASM ───────────────────────────────────────────────────────────────
if [ "$BUILD_PYTHON" -eq 1 ]; then
  section "Python WASM (perf_actor)"
  APP_DIR="$APPS_DIR/python/apps/perf_actor"
  OUT="$APP_DIR/perf_actor.wasm"
  if is_stale "$OUT" "$APP_DIR/perf_actor.py"; then
    echo "  Building Python WASM..."
    bash "$APP_DIR/build.sh" || die "Python WASM build failed"
    ok "perf_actor.wasm → $OUT"
  else
    skip "Python WASM ($(stat -f '%Sm' "$OUT" 2>/dev/null || stat -c '%y' "$OUT" 2>/dev/null))"
  fi
fi

# ── Go WASM ───────────────────────────────────────────────────────────────────
if [ "$BUILD_GO" -eq 1 ]; then
  section "Go WASM (perf_actor)"
  APP_DIR="$APPS_DIR/go/apps/perf_actor"
  OUT="$REPO_ROOT/target/load-test/go/perf_actor/perf_actor.wasm"
  if is_stale "$OUT" "$APP_DIR/perf_actor.go" "$APP_DIR/go.mod"; then
    echo "  Building Go WASM..."
    bash "$APP_DIR/build.sh" || die "Go WASM build failed"
    ok "perf_actor.wasm → $OUT"
  else
    skip "Go WASM ($(stat -f '%Sm' "$OUT" 2>/dev/null || stat -c '%y' "$OUT" 2>/dev/null))"
  fi
fi

# ── TypeScript WASM ───────────────────────────────────────────────────────────
if [ "$BUILD_TS" -eq 1 ]; then
  section "TypeScript WASM (perf_actor)"
  APP_DIR="$APPS_DIR/typescript/apps/perf_actor"
  OUT_DIR="$REPO_ROOT/target/load-test/typescript/perf_actor"
  OUT="$OUT_DIR/perf_actor.wasm"
  if is_stale "$OUT" "$APP_DIR/perf_actor.ts" "$APP_DIR/package.json"; then
    echo "  Building TypeScript WASM..."
    bash "$APP_DIR/build.sh" || die "TypeScript WASM build failed"
    ok "perf_actor.wasm → $OUT"
  else
    skip "TypeScript WASM ($(stat -f '%Sm' "$OUT" 2>/dev/null || stat -c '%y' "$OUT" 2>/dev/null))"
  fi
fi

# ── Rust WASM ─────────────────────────────────────────────────────────────────
if [ "$BUILD_RUST_WASM" -eq 1 ]; then
  section "Rust WASM (perf_actor)"
  APP_DIR="$APPS_DIR/rust/apps/perf_actor"
  OUT_DIR="$REPO_ROOT/target/load-test/rust-wasm/perf_actor"
  OUT="$OUT_DIR/perf_actor.wasm"
  if is_stale "$OUT" "$APP_DIR/Cargo.toml" "$APP_DIR/src/lib.rs"; then
    echo "  Building Rust WASM..."
    bash "$APP_DIR/build.sh" || die "Rust WASM build failed"
    ok "perf_actor.wasm → $OUT"
  else
    skip "Rust WASM ($(stat -f '%Sm' "$OUT" 2>/dev/null || stat -c '%y' "$OUT" 2>/dev/null))"
  fi
fi

echo ""
echo "Done. To run load tests:"
echo "  bash load-test/k6/run.sh --start-server --no-auth --mode all --lang all --transport both --vus 100 --duration 2m"
