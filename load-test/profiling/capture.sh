#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Capture CPU flamegraph from a running PlexSpaces or perf_actor process.
#
# USAGE:
#   bash profiling/capture.sh <output_dir> [duration_sec] [pid]
#
# If pid is not provided, the script finds the perf_actor process first,
# then falls back to plexspaces start.
#
# OUTPUT:
#   <output_dir>/flamegraph-cpu-<pid>.svg  — CPU flamegraph via samply or perf
#
# TOOLS:
#   macOS: cargo install samply   (preferred — opens Firefox Profiler UI automatically)
#          OR: cargo install flamegraph  (dtrace-based)
#          OR: built-in 'sample' command (always available on macOS)
#   Linux: sudo apt install linux-tools-generic && cargo install inferno

set -euo pipefail

OUTPUT_DIR="${1:-./reports/profiling}"
DURATION="${2:-30}"
TARGET_PID="${3:-}"

mkdir -p "$OUTPUT_DIR"

# Find target process if not specified
if [ -z "$TARGET_PID" ]; then
  TARGET_PID=$(pgrep -f "perf_actor [0-9]" 2>/dev/null | head -1 || true)
fi
if [ -z "$TARGET_PID" ]; then
  TARGET_PID=$(pgrep -f "plexspaces start" 2>/dev/null | head -1 || true)
fi

if [ -z "$TARGET_PID" ]; then
  echo "WARNING: No running PlexSpaces process found. Skipping flamegraph." >&2
  exit 0
fi

echo "Profiling PID=$TARGET_PID for ${DURATION}s → $OUTPUT_DIR" >&2

# ─── CPU flamegraph ─────────────────────────────────────────────────────────

if command -v samply >/dev/null 2>&1; then
  # samply: best option on macOS — generates Firefox Profiler JSON
  echo "  Using samply (Firefox Profiler format)..." >&2
  PROFILE_JSON="$OUTPUT_DIR/cpu-profile-${TARGET_PID}.json"
  samply record --pid "$TARGET_PID" --duration "$DURATION" \
    --output "$PROFILE_JSON" 2>/dev/null || \
    echo "  NOTE: samply requires root or entitlements on macOS. Try: sudo samply record ..." >&2
  [ -f "$PROFILE_JSON" ] && echo "  Saved: $PROFILE_JSON (open at https://profiler.firefox.com)" >&2

elif command -v perf >/dev/null 2>&1; then
  # Linux: perf + inferno
  echo "  Using perf + inferno (${DURATION}s)..." >&2
  PERF_DATA="$OUTPUT_DIR/perf-${TARGET_PID}.data"
  sudo perf record -g -F 99 -p "$TARGET_PID" -o "$PERF_DATA" -- sleep "$DURATION" 2>/dev/null || true

  if [ -f "$PERF_DATA" ]; then
    SVG="$OUTPUT_DIR/flamegraph-cpu-${TARGET_PID}.svg"
    if command -v inferno-collapse-perf >/dev/null 2>&1; then
      sudo perf script -i "$PERF_DATA" | inferno-collapse-perf | inferno-flamegraph > "$SVG"
      echo "  Saved: $SVG" >&2
    elif command -v stackcollapse-perf.pl >/dev/null 2>&1 && command -v flamegraph.pl >/dev/null 2>&1; then
      sudo perf script -i "$PERF_DATA" | stackcollapse-perf.pl | flamegraph.pl > "$SVG"
      echo "  Saved: $SVG" >&2
    else
      echo "  NOTE: Install cargo install inferno for SVG output. Data at $PERF_DATA" >&2
    fi
    sudo rm -f "$PERF_DATA"
  fi

elif command -v sample >/dev/null 2>&1; then
  # macOS built-in: sample command, no tools required
  SAMPLE_DURATION=$((DURATION > 30 ? 30 : DURATION))
  echo "  Using macOS sample (${SAMPLE_DURATION}s)..." >&2
  SAMPLE_OUT="$OUTPUT_DIR/cpu-sample-${TARGET_PID}.txt"
  sample "$TARGET_PID" "$SAMPLE_DURATION" -file "$SAMPLE_OUT" 2>/dev/null || true

  if [ -f "$SAMPLE_OUT" ]; then
    echo "  Saved: $SAMPLE_OUT" >&2
    # Convert to flamegraph SVG if cargo flamegraph available
    if command -v flamegraph >/dev/null 2>&1; then
      SVG="$OUTPUT_DIR/flamegraph-cpu-${TARGET_PID}.svg"
      flamegraph "$SAMPLE_OUT" > "$SVG" 2>/dev/null && echo "  Converted to: $SVG" >&2 || true
    else
      echo "  Install: cargo install flamegraph   for SVG output" >&2
      # Parse the sample output to show top 10 functions
      echo "  Top 10 functions by weight:" >&2
      grep -E "^\s+[0-9]+ " "$SAMPLE_OUT" 2>/dev/null | \
        sort -rn | head -10 | awk '{print "    "$1" "$2}' >&2 || true
    fi
  fi

else
  echo "WARNING: No profiling tool available." >&2
  echo "  macOS: cargo install samply" >&2
  echo "  Linux: sudo apt install linux-tools-generic && cargo install inferno" >&2
fi

echo "Done. Files in $OUTPUT_DIR:" >&2
ls -lh "$OUTPUT_DIR/" 2>/dev/null | grep -E "flamegraph|sample|profile|perf" || echo "  (none)" >&2
